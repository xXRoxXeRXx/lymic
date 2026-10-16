# Lymic

Lymic is a desktop client that brings local media into an Immich library. It owns the path from a local folder to a remote asset; optionally on Windows, it projects the remote library as a read-only Virtual Library without performing bidirectional filesystem mirroring.

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
A read-only projection of the Immich Library into the Windows filesystem using the Windows Cloud Files API. Remote Assets appear as on-demand placeholders and are hydrated when opened.
_Avoid_: sync drive, cloud mirror, network share

**Virtual Placeholder**:
A file in the Virtual Library that represents a Remote Asset without storing its complete content locally until opened or pinned.
_Avoid_: cloud file, virtual file, stub

**Hydration**:
The process of downloading the full content of a Remote Asset into a Virtual Placeholder upon user access or pinning.
_Avoid_: download, fetch, caching
