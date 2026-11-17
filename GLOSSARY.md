# Lymic

Lymic is a desktop client that brings local media into an Immich Library. It owns the path from a Watched Folder to a Remote Asset through one-way Synchronization; a planned, Windows-only Virtual Library would project the Immich Library as read-only without performing bidirectional filesystem mirroring.

## Sources and destinations

**Watched Folder**:
A configured local directory whose supported media Lymic discovers and keeps under observation. A Watched Folder may have one Target Album.
_Avoid_: sync folder, source folder, monitored folder

**Removable Watched Folder**:
A Watched Folder located on removable storage, bound to a Volume Identifier and relative path so Lymic can follow mount point changes.
_Avoid_: removable source, external folder, camera source

**Volume Identifier**:
An opaque, operating-system-provided stable identifier for a storage volume that persists across reboots, mount paths, and drive letters.
_Avoid_: drive letter, mount point, volume label

**Immich Server**:
The configured Immich deployment that receives media from Lymic.
_Avoid_: cloud, backend, remote folder

**Immich Library**:
The collection of assets available to the authenticated Immich user on an Immich Server.
_Avoid_: remote filesystem, destination folder

**Target Album**:
An optional album in the Immich Library associated with one Watched Folder. Every asset discovered in that folder is intended to belong to this album.
_Avoid_: target folder, sync album, destination album

**Album**:
A user-visible collection of Remote Assets in the Immich Library. An Album can be owned by or shared with the authenticated user; a Target Album is an Album used for Watched Folder assignment.
_Avoid_: folder, directory, sync album

**Exclusion Pattern**:
A user-defined rule on a Watched Folder that excludes matching paths from Media Asset discovery.
_Avoid_: ignore rule, filter, exclusion filter

## Media

**Media Asset**:
A supported local image or video eligible for discovery by Lymic.
_Avoid_: file, media file, upload file

**Upload Unit**:
The indivisible media item Lymic processes: either one Media Asset or one image with its matching Live Photo Video.
_Avoid_: file pair, upload batch, asset group

**Live Photo Video**:
The video component of an eligible image-and-video Upload Unit. It is associated with its image rather than treated as an unrelated upload.
_Avoid_: companion file, motion file

**XMP Sidecar**:
Optional metadata stored in an XMP file alongside a Media Asset and supplied with that asset when supported.
_Avoid_: metadata file, sidecar file

**Remote Asset**:
An asset already stored in the Immich Library, identified by Immich.
_Avoid_: uploaded file, server file, remote file

**Duplicate**:
A locally discovered Media Asset that Immich recognizes as an existing Remote Asset. A Duplicate is not uploaded again.
_Avoid_: duplicate file, already-uploaded file

**Trashed Duplicate**:
A Duplicate whose Remote Asset is a Trashed Asset. Synchronization treats it as synchronized and leaves the Remote Asset in the Immich trash: Lymic never uploads it again and never restores it. It still receives Album Assignment where Immich allows, and the activity log reports the file as in the Immich trash so the user can restore it in the Trash Timeline (ADR-0010).
_Avoid_: deleted duplicate, re-upload candidate

## Synchronization

**Synchronization**:
The process of discovering Media Assets in Watched Folders, determining their corresponding Remote Assets, uploading missing assets, and applying Target Album membership where configured.
_Avoid_: backup, mirror, two-way sync

**Album Assignment**:
Adding a Remote Asset to the Target Album of the Watched Folder that produced it. This applies to both newly uploaded assets and Duplicates.
_Avoid_: album tagging, album upload

**Queued Synchronization**:
Synchronization work retained by Lymic so that it can pause, resume, and recover without losing the outcome required for an Upload Unit.
_Avoid_: upload queue, job queue

**Failed Synchronization**:
An Upload Unit whose required Synchronization outcome was not reached and is available for retry.
_Avoid_: failed file, upload error

## Virtual Library

**Virtual Library**:
A planned, Windows-only read-only projection of the Immich Library into the Windows filesystem using the Windows Cloud Files API, separate from Synchronization. Remote Assets appear as on-demand Virtual Placeholders and undergo Hydration when opened.
_Avoid_: sync drive, cloud mirror, network share

**Virtual Placeholder**:
A file in the Virtual Library that represents a Remote Asset without storing its complete content locally until opened or pinned.
_Avoid_: cloud file, virtual file, stub

**Hydration**:
The process of downloading the full content of a Remote Asset into a Virtual Placeholder upon user access or pinning.
_Avoid_: fetch, caching

## Photo Timeline

**Download**:
The user-initiated retrieval and saving of a Remote Asset's original file to a designated local file path.
_Avoid_: export, save image, rip

**Favorite**:
A user-toggled status marking a Remote Asset as favored in the Immich Library.
_Avoid_: star, bookmark, like

**Recognized Person**:
An identity group created by Immich that has an optional assigned name and associated Remote Assets.
_Avoid_: person, face, face group

**Photo Timeline**:
The chronological presentation of Remote Assets in the Immich Library grouped into time buckets. Archived Assets are not part of it, as in Immich Web; they appear in the Archive Timeline.
_Avoid_: gallery, photo feed, media stream

**Timeline Bucket**:
A discrete time interval (such as a month) returned by Immich, containing asset counts and identifiers for assets captured during that period.
_Avoid_: time slice, photo group, date cluster

**Asset Thumbnail**:
A downscaled preview image of a Remote Asset generated and served by the Immich Server.
_Avoid_: preview image, mini image, thumb

**Thumbnail Cache**:
A size-bounded local disk cache maintained by Lymic for Asset Thumbnails to eliminate repeated network fetches.
_Avoid_: image cache, photo store

**Timeline Scrubber**:
A vertical control alongside the Photo Timeline displaying years and months to enable fast jumping to any time period.
_Avoid_: date slider, fast scroll bar, year index

**Album Timeline**:
The chronological presentation of the Remote Assets belonging to one Album, grouped into time buckets like the Photo Timeline.
_Avoid_: album grid, album gallery

**Favorites Timeline**:
The chronological presentation of Remote Assets marked as Favorites in the authenticated user's Immich Library. Like the Photo Timeline it leaves out Archived Assets.
_Avoid_: favorites album, favorites folder

**Archive Timeline**:
The chronological presentation of the Remote Assets that are Archived Assets in the authenticated user's Immich Library, grouped into time buckets like the Photo Timeline. It is read-only: Lymic neither archives nor unarchives a Remote Asset.
_Avoid_: archive folder, archive album, hidden gallery

**Trashed Asset**:
A Remote Asset that sits in the Immich trash after being deleted in Immich, until Immich removes it permanently or it is restored. Lymic only reads this status and may restore a Trashed Asset; it never moves a Remote Asset into the trash and never deletes one permanently. Not related to the Windows Recycle Bin or to removing a Watched Folder.
_Avoid_: deleted asset, recycle bin item, bin item

**Trash Timeline**:
The chronological presentation of the authenticated user's Trashed Assets, grouped into time buckets like the Photo Timeline. Restoring a Trashed Asset returns it to the Immich Library; it does not touch local files, Watched Folders or Synchronization state.
_Avoid_: recycle bin, trash folder, deleted gallery

**Restore**:
Returning a Trashed Asset to the authenticated user's Immich Library, from the Trash Timeline Lightbox or a multi-selection. It is the only Immich write the Trash Timeline performs; Lymic never moves a Remote Asset into the trash and never deletes one permanently. Trashed Assets that could not be restored stay visible and selected in the Trash Timeline so the user can retry.
_Avoid_: undelete, recover, untrash

## Map

**Map View**:
The geographic presentation of Remote Assets with a recorded location in the Immich Library, including both images and videos.
_Avoid_: photo map, upload map, location gallery

## Explore

**Explore Overview**:
The entry view that gathers curated sections of the Immich Library (Recognized People, Places, and Memories) and leads into the view that presents each one in full. It owns no data of its own.
_Avoid_: dashboard, discover, home

**Place**:
A city in which the authenticated user has Remote Assets, represented in the Explore Overview by one Remote Asset's Asset Thumbnail and the city name. A Place is identified by its city name alone, so same-named cities in different countries are one Place (ADR-0007).
_Avoid_: location, destination, map pin

**Place Timeline**:
The chronological presentation of the Remote Assets recorded in one Place, grouped into time buckets like the Photo Timeline. Assets without a usable capture time are omitted.
_Avoid_: city gallery, location feed

**Memory**:
An Immich-generated, dated selection of Remote Assets, such as photos from a previous year on the same day or a Recognized Person's birthday, shown in the Explore Overview. Lymic only reads Memories; it never creates, edits or deletes them.
_Avoid_: story, highlight, flashback

**Memory Timeline**:
The chronological presentation of the Remote Assets belonging to one Memory, grouped into time buckets like the Photo Timeline. Assets without a usable capture time are omitted.
_Avoid_: memory slideshow, memory player

## Tags

**Tag**:
A named, optionally hierarchical label that Immich attaches to Remote Assets. A Tag is identified by its full path (for example `Travel/Italy/Rome`), so a Tag can have a parent Tag and child Tags; its own name is the last part of that path. Lymic only reads Tags, to browse them and to show them on a Remote Asset; it never creates, edits, assigns or deletes them.
_Avoid_: label, keyword, category

**Tag Timeline**:
The chronological presentation of the Remote Assets belonging to one Tag, grouped into time buckets like the Photo Timeline. It shows what Immich returns for that Tag; Lymic adds no rule of its own about the assets of its child Tags.
_Avoid_: tag gallery, tag feed

## Search

**Search**:
A read-only query over the Immich Library made of an optional text and any number of Search Filters, answered by Immich. Searching never changes a Remote Asset.
_Avoid_: find, lookup, query

**Search Mode**:
How Immich interprets the text of a Search: by the visual content of Remote Assets (Context, the default), by file name, by description, by full folder path, or by text recognized within the media (OCR). Exactly one Search Mode applies at a time.
_Avoid_: search type, search scope

**Search Filter**:
A condition that narrows the Remote Assets a Search returns, such as Recognized Person, date, Location, Tag, media type, camera, Rating or Favorite status. A Remote Asset must satisfy every Search Filter that is set.
_Avoid_: facet, refinement

**Location**:
A country, a state or region within it, and a city, each optional, used to narrow a Search. Distinct from a Place, which is identified by city name alone.
_Avoid_: place (in a Search), geo filter

**Archived Asset**:
A Remote Asset that Immich hides from its main views. Lymic only reads this status and never sets or clears it. Not related to the archive file produced by a multi-asset Download.
_Avoid_: archive (alone), hidden asset

**Rating**:
The one-to-five star evaluation Immich stores for a Remote Asset. Lymic only reads it.
_Avoid_: stars, score, review

**Unfiled Asset**:
A Remote Asset that belongs to no Album.
_Avoid_: orphan, loose asset, unalbumed asset

**Search Results**:
The Remote Assets a Search returns. Without a Context text they are presented chronologically in Timeline Buckets, like the Photo Timeline; with a Context text they are presented in order of relevance, without Timeline Buckets.
_Avoid_: hits, matches, search timeline
